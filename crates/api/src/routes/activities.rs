use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::{
    types::{
        participant_roles, ItemDelta, ItemKind, LockCallEntry, ParticipantDelta, ParticipantId,
        TxActions, TypeCallEntry,
    },
    CkbadgerStore,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use crate::pool::PoolTxRecord;
use crate::response::{
    default_limit, hash_type_to_str, ok, ApiError, ApiResult, ApiRouteError,
    CursorPaginatedResponse,
};
use crate::routes::address_pool;
use crate::utils::address::{
    address_to_lock_script_hash, compute_script_hash, encode_lock_address, resolve_lock_address,
};
use crate::utils::exact_json::integers_as_strings;
use crate::utils::{
    is_ckb_address, parse_hash32, parse_optional_block_tx_cursor, registered_script_name,
};
use crate::AppState;
use ckbadger_indexer::parser::registry::protocol_display_name;

/// A party's address, from wherever its lock script is known.
///
/// A committed row's parties hold (or held) cells the indexer wrote, so the
/// chain view's `CF_LOCK_SCRIPTS` knows every one of them. A tx-pool row may
/// pay a lock no committed cell has used yet, or spend a cell whose lock exists
/// so far only as the output of another pending transaction; both scripts are
/// on the mirror's record (its outputs and its spent inputs' locks) and are
/// encoded from there — a store miss for a brand-new lock is not "unknown".
/// Either way the address comes from `encode_lock_address`, the one encoder; a
/// lock known to neither is unresolved (`None`), never a fabricated address.
fn resolve_party_address(
    store: &CkbadgerStore,
    network: &str,
    lock_hash: &[u8; 32],
    pool_record: Option<&PoolTxRecord>,
    cache: &mut HashMap<Vec<u8>, Option<String>>,
) -> anyhow::Result<Option<String>> {
    if let Some(lock) = pool_record.and_then(|record| record.lock_script(lock_hash)) {
        return encode_lock_address(
            lock_hash,
            &lock.code_hash,
            lock.hash_type,
            &lock.args,
            network,
        )
        .map(Some);
    }
    if let Some(cached) = cache.get(lock_hash.as_slice()) {
        return Ok(cached.clone());
    }
    let address = resolve_lock_address(store, lock_hash, network)?.map(|resolved| resolved.address);
    cache.insert(lock_hash.to_vec(), address.clone());
    Ok(address)
}

/// One party of a transaction as the API reports it.
///
/// A party the protocol named by a 20-byte lock-hash prefix may or may not be
/// resolvable to a full lock hash, so `address` and `lockHash` are optional and
/// `lockHashPrefix` is what the chain actually says. An unresolved party renders
/// as its prefix — never as a fabricated address.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantRef {
    pub address: Option<String>,
    pub lock_hash: Option<String>,
    pub lock_hash_prefix: Option<String>,
    pub lock_kind: Option<String>,
    pub roles: Vec<&'static str>,
}

pub(crate) fn participant_lock_kind(
    store: &CkbadgerStore,
    lock_hash: &[u8; 32],
    pool_record: Option<&PoolTxRecord>,
) -> anyhow::Result<Option<String>> {
    let code_hash = if let Some(lock) = pool_record.and_then(|record| record.lock_script(lock_hash))
    {
        Some(lock.code_hash)
    } else {
        store.get_lock_script(lock_hash)?.map(|lock| lock.code_hash)
    };
    code_hash
        .map(|hash| registered_script_name(store, &hash))
        .transpose()
        .map(Option::flatten)
}

fn participant_ref(
    store: &CkbadgerStore,
    network: &str,
    participant: &ParticipantDelta,
    pool_record: Option<&PoolTxRecord>,
    cache: &mut HashMap<Vec<u8>, Option<String>>,
) -> anyhow::Result<ParticipantRef> {
    let roles = participant_roles::names(participant.roles);
    match participant.id {
        ParticipantId::Lock(hash) => Ok(ParticipantRef {
            address: resolve_party_address(store, network, &hash, pool_record, cache)?,
            lock_hash: Some(format!("0x{}", hex::encode(hash))),
            lock_hash_prefix: None,
            lock_kind: participant_lock_kind(store, &hash, pool_record)?,
            roles,
        }),
        // A protocol-named prefix resolves at read time through the chain
        // view (spec §3), and only when exactly one known lock has it.
        ParticipantId::LockPrefix(prefix) => match store.resolve_lock_hash_prefix(&prefix)? {
            Some((hash, entry)) => Ok(ParticipantRef {
                address: Some(encode_lock_address(
                    &hash,
                    &entry.code_hash,
                    entry.hash_type,
                    &entry.args,
                    network,
                )?),
                lock_hash: Some(format!("0x{}", hex::encode(hash))),
                lock_hash_prefix: Some(format!("0x{}", hex::encode(prefix))),
                lock_kind: registered_script_name(store, &entry.code_hash)?,
                roles,
            }),
            None => Ok(ParticipantRef {
                address: None,
                lock_hash: None,
                lock_hash_prefix: Some(format!("0x{}", hex::encode(prefix))),
                lock_kind: None,
                roles,
            }),
        },
    }
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/addresses/{addr}/activities", get(get_address_activities))
        .route("/activities", get(get_global_activities))
        .route("/activities/latest", get(get_latest_activities))
        .route(
            "/transactions/{hash}/activity",
            get(get_transaction_activity),
        )
}

#[derive(Debug, Deserialize)]
pub struct ActivityParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    filter: Option<String>,
    #[serde(rename = "blockNumber", alias = "block_number")]
    block_number: Option<i64>,
    /// `asc` (block order) or `desc` (default, newest first); only for a
    /// block-scoped feed (`blockNumber`), whose rows have a block order (BA-14).
    order: Option<String>,
}

/// Per-address activity response: shows one participant's perspective of a transaction.
///
/// Chain position is nullable because the same shape serves transactions that
/// are still in the node's tx pool. Those have no block, no index and no block
/// time — only `poolStatus`, `timeAddedToPool` and how far they could be
/// interpreted. Nothing downstream can mistake one for a committed row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityResponse {
    pub tx_hash: String,
    pub block_number: Option<i64>,
    pub tx_index: Option<i32>,
    pub timestamp: Option<String>,
    /// `pending`, `proposed` or `committed_awaiting_index` for a tx-pool row;
    /// absent for a committed one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_added_to_pool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<crate::pool::InterpretationResponse>,
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
    pub participants: Vec<ParticipantRef>,
    /// Roles the protocol gave THIS participant, if any.
    pub roles: Vec<&'static str>,
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
    Object {
        object_id: String,
        delta: i8,
        /// The object's collection (`transaction_cells::object_delta_context`):
        /// a spore's cluster (id + immutable name; the Sole Spores sentinel
        /// with `name: null` for a clusterless spore), a cluster's own id and
        /// name, an m-NFT token's class id (`name: null`). `null` only on a
        /// pool row whose object is not indexed yet.
        collection: Option<super::transaction_cells::DeltaCollection>,
        /// The site's asset class vocabulary: always `object`.
        class: &'static str,
        /// `CompositionTier::as_str` for a spore; `null` for a Spore cluster,
        /// an m-NFT token and a pool row whose object is not indexed yet.
        composition: Option<&'static str>,
        /// What the object is: `spore | cluster | mnft_token`
        /// (`ObjectDeltaRole::as_str`); `null` only on a pool row whose object
        /// is not indexed yet. A cluster's `collection` is the cluster itself.
        role: Option<&'static str>,
    },
    #[serde(rename = "identity", rename_all = "camelCase")]
    Identity {
        identity_id: String,
        delta: i8,
        /// The identity's standard, as the builder recorded it on the delta,
        /// in its one wire value (`IdentityStandard::asset_standard`:
        /// `dotbit | bit_cell | did_ckb | dotcell`). The frontend links the
        /// item to its standard's page with it.
        standard: &'static str,
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
    /// Decoded lock args; every integer in it is an exact decimal string
    /// (`utils::exact_json`).
    pub decoded: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolActionResponse {
    pub protocol: String,
    /// The protocol's display name from the bundled registry
    /// (`docs/metadata/protocols.toml`), the one name every client shows.
    pub protocol_name: String,
    pub action: String,
    /// Protocol-specific JSON; every integer in it is an exact decimal
    /// string (`utils::exact_json`).
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
    pub address: Option<String>,
    pub lock_hash: Option<String>,
    pub lock_hash_prefix: Option<String>,
    pub lock_kind: Option<String>,
    pub roles: Vec<&'static str>,
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

/// A token's `(symbol, decimals)`, `None` when no token row exists.
type TokenDisplay = Option<(Option<String>, Option<u8>)>;

/// What one response has already read for its item deltas: token symbol /
/// decimals, and object delta contexts keyed by `(object id, indexed)` — a
/// pool row's lookup (which may legitimately find no row yet) never answers
/// for a committed row's.
#[derive(Default)]
pub(crate) struct ItemDeltaCache {
    tokens: HashMap<Vec<u8>, TokenDisplay>,
    objects: HashMap<(Vec<u8>, bool), super::transaction_cells::ObjectDeltaContext>,
}

/// Convert an `ItemDelta` to `ItemDeltaResponse`: tokens gain their
/// symbol/decimals, objects their collection, class and composition. The one
/// seam every producer of item deltas (address / global / transaction
/// activity, asset feeds, the pool mirror) goes through. `indexed` is false
/// only for a pool (pending) row.
fn convert_item_delta(
    item: &ItemDelta,
    cache: &mut ItemDeltaCache,
    store: &CkbadgerStore,
    indexed: bool,
) -> anyhow::Result<ItemDeltaResponse> {
    match item.kind {
        ItemKind::Token => {
            let (symbol, decimals) = lookup_token_info(store, &mut cache.tokens, &item.item_id)?;
            Ok(ItemDeltaResponse::Token {
                type_script_hash: format!("0x{}", hex::encode(&item.item_id)),
                delta: format!("{}{}", if item.negative { "-" } else { "" }, item.magnitude),
                symbol,
                decimals,
            })
        }
        ItemKind::Object => {
            let delta = discrete_item_delta(item)?;
            let key = (item.item_id.clone(), indexed);
            let context = match cache.objects.get(&key) {
                Some(context) => context.clone(),
                None => {
                    let context = super::transaction_cells::object_delta_context(
                        store,
                        &item.item_id,
                        indexed,
                    )?;
                    cache.objects.insert(key, context.clone());
                    context
                }
            };
            Ok(ItemDeltaResponse::Object {
                object_id: format!("0x{}", hex::encode(&item.item_id)),
                delta,
                collection: context.collection,
                class: context.class,
                composition: context.composition,
                role: context.role,
            })
        }
        ItemKind::Identity(standard) => Ok(ItemDeltaResponse::Identity {
            identity_id: format!("0x{}", hex::encode(&item.item_id)),
            delta: discrete_item_delta(item)?,
            standard: standard.asset_standard(),
        }),
    }
}

fn discrete_item_delta(item: &ItemDelta) -> anyhow::Result<i8> {
    if item.magnitude != 1 {
        anyhow::bail!(
            "activity discrete-item invariant violated: kind={:?} item=0x{} magnitude={} (expected 1)",
            item.kind,
            hex::encode(&item.item_id),
            item.magnitude
        );
    }
    Ok(if item.negative { -1 } else { 1 })
}

/// Look up token symbol and decimals from CF_TOKENS, caching results.
fn lookup_token_info(
    store: &CkbadgerStore,
    cache: &mut HashMap<Vec<u8>, TokenDisplay>,
    type_script_hash: &[u8],
) -> anyhow::Result<(Option<String>, Option<u8>)> {
    if let Some(cached) = cache.get(type_script_hash) {
        return Ok(cached
            .as_ref()
            .map(|(s, d)| (s.clone(), *d))
            .unwrap_or((None, None)));
    }
    let result = store
        .get_token(type_script_hash)
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to read token 0x{} for activity: {e}",
                hex::encode(type_script_hash)
            )
        })?
        .map(|t| (t.symbol.clone(), t.decimals.map(|d| d as u8)));
    cache.insert(type_script_hash.to_vec(), result.clone());
    Ok(result.unwrap_or((None, None)))
}

/// Registered script names already read by one response, keyed by code hash
/// (`utils::registered_script_name`, the one name rule `/cells` also uses).
pub(crate) type ScriptNameCache = HashMap<Vec<u8>, Option<String>>;

fn cached_script_name(
    store: &CkbadgerStore,
    cache: &mut ScriptNameCache,
    code_hash: &[u8],
) -> anyhow::Result<Option<String>> {
    if let Some(name) = cache.get(code_hash) {
        return Ok(name.clone());
    }
    let name = registered_script_name(store, code_hash)?;
    cache.insert(code_hash.to_vec(), name.clone());
    Ok(name)
}

fn convert_type_call(
    store: &CkbadgerStore,
    cache: &mut ScriptNameCache,
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
    let script_name = cached_script_name(store, cache, &call.type_code_hash)?;

    Ok(TypeCallResponse {
        type_code_hash: format!("0x{}", hex::encode(&call.type_code_hash)),
        type_hash_type: hash_type.to_string(),
        type_args: format!("0x{}", hex::encode(&call.type_args)),
        script_hash: format!("0x{}", hex::encode(script_hash)),
        script_name,
    })
}

fn convert_type_calls(
    store: &CkbadgerStore,
    cache: &mut ScriptNameCache,
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
    // The ordinary lock families (TX-20); each code hash is a
    // `canonical_ref_hash` of its `docs/metadata/scripts/<slug>.toml`, which
    // `lock_args_decoders_follow_the_script_registry` checks.
    for (decoder, hexes) in [
        (
            decode_secp256k1_blake160_args as ArgsDecoder,
            &["0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"][..], // default-lock
        ),
        (
            decode_multisig_args as ArgsDecoder,
            &[
                "0x5c5069eb0857efc65e1bca0c07df34c31663b3622fd3876c876320fc9634e2a8",
                "0xd1a9f877aed3f5e07cb9c52b61ab96d06f250ae6883cc7f0a2423db0976fc821",
                "0x36c971b8d41fbd94aabca77dc75e826729ac98447b46f91e00796155dddb0d29",
                "0x765b3ed6ae264b335d07e73ac332bf2c0f38f8d3340ed521cb447b4c42dd5f09",
            ][..], // default-multisig
        ),
        (
            decode_anyone_can_pay_args as ArgsDecoder,
            &[
                "0xd369597ff47f29fbc0d47d2e3775370d1250b85140c670e4718af712983a2354",
                "0x3419a1c09eb2567f6552ee7a8ecffd64155cffe0f1796e6e61ec088d740c1356",
                "0x86a1c6987a4acbe1a887cca4c9dd2ac9fcb07405bbeda51b861b18bbf7492c4b",
            ][..], // anyone-can-pay-lock
        ),
        (
            decode_omnilock_args as ArgsDecoder,
            &[
                "0x9b819793a64463aed77c615d6cb226eea5487ccfc0783043a587254cda2b6f26",
                "0xa4398768d87bd17aea1361edc3accd6a0117774dc4ebc813bfa173e8ac0d086d",
                "0xf329effd1c475a2978453c8600e1eaf0bc2087ee093c3ee64cc96ec6847752cb",
                "0x79f90bb5e892d80dd213439eeab551120eb417678824f282b4ffb5f21bad2e1e",
            ][..], // omni-lock
        ),
    ] {
        for hex in hexes {
            m.insert(parse_hex_code_hash(hex), decoder);
        }
    }
    m
});

/// The one lock-args decoder (activity lock calls and the transaction
/// detail): the decoded args of a lock with code hash `code_hash`, integers
/// as exact strings; `None` for a lock family with no known args layout or
/// args that do not fit it.
pub(crate) fn decode_lock_args(
    code_hash: &[u8],
    args: &[u8],
) -> Result<Option<serde_json::Value>, String> {
    LOCK_ARGS_DECODERS
        .get(code_hash)
        .and_then(|decoder| decoder(args))
        .map(|decoded| integers_as_strings(decoded, "decoded lock args"))
        .transpose()
}

/// secp256k1-blake160-sighash-all: args are the 20-byte blake160 of the
/// public key.
fn decode_secp256k1_blake160_args(args: &[u8]) -> Option<serde_json::Value> {
    (args.len() == 20).then(|| {
        serde_json::json!({
            "lock": "secp256k1_blake160",
            "pubkeyHash": format!("0x{}", hex::encode(args)),
        })
    })
}

/// secp256k1-blake160-multisig-all: the 20-byte blake160 of the multisig
/// script, optionally followed by an 8-byte `since` (u64 LE) the inputs must
/// satisfy.
fn decode_multisig_args(args: &[u8]) -> Option<serde_json::Value> {
    let mut decoded = serde_json::json!({
        "lock": "multisig",
        "multisigScriptHash": format!("0x{}", hex::encode(args.get(..20)?)),
    });
    match args.len() {
        20 => {}
        28 => {
            decoded["since"] = serde_json::json!(u64::from_le_bytes(args[20..28].try_into().ok()?))
        }
        _ => return None,
    }
    Some(decoded)
}

/// anyone-can-pay (RFC-0026): a 20-byte pubkey hash, then optionally the
/// minimum-CKB exponent and the minimum-UDT exponent, one byte each.
fn decode_anyone_can_pay_args(args: &[u8]) -> Option<serde_json::Value> {
    if !(20..=22).contains(&args.len()) {
        return None;
    }
    let mut decoded = serde_json::json!({
        "lock": "anyone_can_pay",
        "pubkeyHash": format!("0x{}", hex::encode(&args[..20])),
    });
    if let Some(exponent) = args.get(20) {
        decoded["minimumCkbExponent"] = serde_json::json!(exponent);
    }
    if let Some(exponent) = args.get(21) {
        decoded["minimumUdtExponent"] = serde_json::json!(exponent);
    }
    Some(decoded)
}

/// Omnilock (RFC-0042): a 21-byte auth (flag + 20-byte content), the
/// omnilock flags byte, then exactly the optional fields the flags turn on,
/// in order — administrator RC cell type ID (0x1, 32 B), anyone-can-pay
/// minimums (0x2, 2 B), time-lock `since` (0x4, 8 B LE), supply type script
/// hash (0x8, 32 B).
fn decode_omnilock_args(args: &[u8]) -> Option<serde_json::Value> {
    let flag = *args.first()?;
    let flags = *args.get(21)?;
    if flags & !0x0f != 0 {
        return None;
    }
    let auth_kind = match flag {
        0x00 => Some("secp256k1_blake160"),
        0x01 => Some("ethereum"),
        0x02 => Some("eos"),
        0x03 => Some("tron"),
        0x04 => Some("bitcoin"),
        0x05 => Some("dogecoin"),
        0x06 => Some("ckb_multisig"),
        0x07 => Some("ethereum_display"),
        0xfc => Some("owner_lock"),
        0xfd => Some("exec"),
        0xfe => Some("dynamic_linking"),
        _ => None,
    };
    let mut decoded = serde_json::json!({
        "lock": "omnilock",
        "authFlag": flag,
        "authKind": auth_kind,
        "authContent": format!("0x{}", hex::encode(&args[1..21])),
        "omnilockFlags": flags,
    });
    let mut rest = &args[22..];
    let mut take = |len: usize| -> Option<&[u8]> {
        let (field, tail) = (rest.get(..len)?, rest.get(len..)?);
        rest = tail;
        Some(field)
    };
    if flags & 0x1 != 0 {
        decoded["adminListCellTypeId"] = serde_json::json!(format!("0x{}", hex::encode(take(32)?)));
    }
    if flags & 0x2 != 0 {
        let minimums = take(2)?;
        decoded["minimumCkbExponent"] = serde_json::json!(minimums[0]);
        decoded["minimumUdtExponent"] = serde_json::json!(minimums[1]);
    }
    if flags & 0x4 != 0 {
        decoded["since"] = serde_json::json!(u64::from_le_bytes(take(8)?.try_into().ok()?));
    }
    if flags & 0x8 != 0 {
        decoded["supplyTypeScriptHash"] =
            serde_json::json!(format!("0x{}", hex::encode(take(32)?)));
    }
    rest.is_empty().then_some(decoded)
}

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

/// Decodes UTXOSwap intent lock args for live display.
///
/// Both the field layout and the field names come from the single shared
/// decoder in `ckbadger-common`, so this display can never drift from the
/// metadata the indexer persists. Only the `protocol` tag is added here.
fn decode_utxoswap_intent_args(args: &[u8]) -> Option<serde_json::Value> {
    let mut result = ckbadger_common::utxoswap::parse_intent_args(args)?.metadata_json();
    result["protocol"] = serde_json::json!("utxoswap");
    Some(result)
}

fn convert_lock_call(
    store: &CkbadgerStore,
    cache: &mut ScriptNameCache,
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
    let script_name = cached_script_name(store, cache, &call.lock_code_hash)?;

    Ok(LockCallResponse {
        lock_code_hash: format!("0x{}", hex::encode(&call.lock_code_hash)),
        lock_hash_type: hash_type.to_string(),
        lock_args: format!("0x{}", hex::encode(&call.lock_args)),
        script_hash: format!("0x{}", hex::encode(script_hash)),
        script_name,
        decoded: decode_lock_args(&call.lock_code_hash, &call.lock_args)
            .map_err(anyhow::Error::msg)?,
    })
}

fn convert_lock_calls(
    store: &CkbadgerStore,
    cache: &mut ScriptNameCache,
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

    let protocol_name = protocol_display_name(&action.protocol).ok_or_else(|| {
        anyhow::anyhow!(
            "protocol action {}/{} has no display name in docs/metadata/protocols.toml",
            action.protocol,
            action.action
        )
    })?;
    let context = format!("{}/{} protocol metadata", action.protocol, action.action);
    Ok(ProtocolActionResponse {
        protocol: action.protocol.clone(),
        protocol_name: protocol_name.to_string(),
        action: action.action.clone(),
        metadata: integers_as_strings(metadata, &context).map_err(anyhow::Error::msg)?,
    })
}

/// What a pool row adds to an activity response.
///
/// Its presence is what turns the chain-position fields null: a transaction the
/// pool holds has no block to report, and reporting the mirror's provisional
/// zeros would let a caller mistake it for genesis.
pub(crate) struct PoolRowMeta<'a> {
    pub status: String,
    pub time_added_to_pool: String,
    pub interpretation: crate::pool::InterpretationResponse,
    /// The mirror's record of the pool transaction: its outputs and its spent
    /// inputs' lock scripts, which the chain view may not know yet.
    pub record: &'a PoolTxRecord,
}

/// Build an address-scoped activity response from a TxActions for a specific participant.
///
/// The SAME builder serves committed rows and tx-pool rows: a transaction must
/// read identically before and after commit, and only its truth status changes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn build_activity_response(
    store: &CkbadgerStore,
    network: &str,
    actions: &TxActions,
    participant: &ParticipantDelta,
    script_info_cache: &mut ScriptNameCache,
    item_cache: &mut ItemDeltaCache,
    address_cache: &mut HashMap<Vec<u8>, Option<String>>,
    pool: Option<PoolRowMeta<'_>>,
) -> anyhow::Result<ActivityResponse> {
    let item_deltas = participant
        .item_deltas
        .iter()
        .map(|item| convert_item_delta(item, item_cache, store, pool.is_none()))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let participants = actions
        .participants
        .iter()
        .filter(|p| p.id != participant.id)
        .map(|p| {
            participant_ref(
                store,
                network,
                p,
                pool.as_ref().map(|meta| meta.record),
                address_cache,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(ActivityResponse {
        tx_hash: format!("0x{}", hex::encode(&actions.tx_hash)),
        block_number: pool.is_none().then_some(actions.block_number),
        tx_index: pool.is_none().then_some(actions.tx_index),
        timestamp: pool.is_none().then(|| actions.timestamp.to_string()),
        pool_status: pool.as_ref().map(|meta| meta.status.clone()),
        time_added_to_pool: pool.as_ref().map(|meta| meta.time_added_to_pool.clone()),
        interpretation: pool.as_ref().map(|meta| meta.interpretation.clone()),
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
        roles: participant_roles::names(participant.roles),
        tags: participant.tags,
    })
}

/// Build a global activity response showing all participants from a TxActions.
#[allow(clippy::type_complexity)]
pub(crate) fn build_global_activity_response(
    store: &CkbadgerStore,
    _ao_store: &CkbadgerStore,
    network: &str,
    actions: &TxActions,
    script_info_cache: &mut ScriptNameCache,
    address_cache: &mut HashMap<Vec<u8>, Option<String>>,
) -> anyhow::Result<GlobalActivityResponse> {
    build_global_activity_response_with_pool(
        store,
        _ao_store,
        network,
        actions,
        script_info_cache,
        address_cache,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_global_activity_response_with_pool(
    store: &CkbadgerStore,
    _ao_store: &CkbadgerStore,
    network: &str,
    actions: &TxActions,
    script_info_cache: &mut ScriptNameCache,
    address_cache: &mut HashMap<Vec<u8>, Option<String>>,
    pool_record: Option<&PoolTxRecord>,
) -> anyhow::Result<GlobalActivityResponse> {
    let mut item_cache = ItemDeltaCache::default();

    let participants = actions
        .participants
        .iter()
        .map(|p| {
            let reference = participant_ref(store, network, p, pool_record, address_cache)?;
            let item_deltas = p
                .item_deltas
                .iter()
                .map(|item| convert_item_delta(item, &mut item_cache, store, pool_record.is_none()))
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(ParticipantResponse {
                address: reference.address,
                lock_hash: reference.lock_hash,
                lock_hash_prefix: reference.lock_hash_prefix,
                lock_kind: reference.lock_kind,
                roles: reference.roles,
                ckb_delta: p.ckb_delta.to_string(),
                used_delta: p.used_delta.to_string(),
                item_deltas,
                tags: p.tags,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

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
                | "object"
                | "identity"
                | "dao"
                | "type_call"
                | "lock_call"
                | "script"
        ) && !value.starts_with("protocol:")
        {
            return Err(ApiError::bad_request(format!(
                "invalid activity filter '{}'; expected one of: all, ckb, token, object, identity, dao, type_call, lock_call, script, protocol:<name>",
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

/// Whether a `TxActions` falls under a global activity filter. The bucket comes
/// from `ckbadger_store::classify_global_bucket`, the one classification the
/// indexer also keys `CF_TX_ACTIONS_BY_BUCKET` by; `cellbase` is the one
/// filter that names no bucket.
pub(crate) fn matches_global_activity_filter(actions: &TxActions, filter: Option<&str>) -> bool {
    match filter {
        None | Some("") | Some("all") => true,
        Some("cellbase") => actions.is_cellbase,
        Some(expected) => ckbadger_store::classify_global_bucket(actions)
            .is_some_and(|bucket| bucket.as_filter() == expected),
    }
}

/// List canonical global activities (TX-level), newest first, strictly older
/// than `cursor`.
///
/// A bucket filter reads `CF_TX_ACTIONS_BY_BUCKET`, which the indexer writes
/// beside every non-cellbase `TxActions` row and deletes above the fork point
/// on rollback: each row is canonical by construction and a page costs one
/// position seek per row. The unfiltered feed walks `CF_TX_ACTIONS`.
pub(crate) fn list_canonical_global_actions_page(
    store: &CkbadgerStore,
    limit: usize,
    cursor: Option<(i64, i32)>,
    filter: Option<&str>,
) -> anyhow::Result<Vec<TxActions>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    match filter {
        None | Some("") | Some("all") => {
            list_canonical_unfiltered_actions_page(store, limit, cursor)
        }
        Some(value) => {
            let bucket = ckbadger_store::GlobalBucket::from_filter(value).ok_or_else(|| {
                anyhow::anyhow!("global activity filter '{value}' names no activity bucket")
            })?;
            store.list_tx_actions_page_by_bucket(bucket, cursor, limit)
        }
    }
}

fn list_canonical_unfiltered_actions_page(
    store: &CkbadgerStore,
    limit: usize,
    cursor: Option<(i64, i32)>,
) -> anyhow::Result<Vec<TxActions>> {
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

/// Build this lock's tx-pool rows for page one, and the summary describing
/// them: the shared page-one segment, narrowed by this feed's filter.
///
/// Blocking (reads the store for dedup and for script/address resolution).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn build_pool_activity_rows(
    store: &CkbadgerStore,
    network: &str,
    snapshot: &crate::pool::PoolSnapshot,
    lock_hash: &[u8; 32],
    filter: Option<&str>,
    script_info_cache: &mut ScriptNameCache,
    item_cache: &mut ItemDeltaCache,
    address_cache: &mut HashMap<Vec<u8>, Option<String>>,
) -> anyhow::Result<(Vec<ActivityResponse>, crate::pool::PoolSummaryResponse)> {
    let segment = address_pool::page_one_segment(store, snapshot, lock_hash)?;

    let mut rows = Vec::with_capacity(segment.rows.len());
    let mut served = Vec::with_capacity(segment.rows.len());
    for row in &segment.rows {
        let record = &row.record;
        // `by_lock` is built from a record's interpreted participants, so a
        // record reached through it must have an interpretation. A missing one
        // is a mirror invariant violation, not a row to skip quietly.
        let actions = record.actions.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "pool record indexed by lock 0x{} carries no interpretation: tx=0x{}",
                hex::encode(lock_hash),
                hex::encode(record.tx_hash)
            )
        })?;
        // The same filter → tag-bitmap mapping the committed index uses.
        if !CkbadgerStore::matches_activity_filter(actions, lock_hash, filter) {
            continue;
        }
        let participant = actions
            .participants
            .iter()
            .find(|p| p.id.matches(lock_hash))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "pool record indexed by lock 0x{} has no participant for it: tx=0x{}",
                    hex::encode(lock_hash),
                    hex::encode(record.tx_hash)
                )
            })?;

        rows.push(build_activity_response(
            store,
            network,
            actions,
            participant,
            script_info_cache,
            item_cache,
            address_cache,
            Some(PoolRowMeta {
                status: record.pool_status.as_str().to_string(),
                time_added_to_pool: crate::pool::pool_timestamp_rfc3339(i64::try_from(
                    record.entry.time_added_to_pool_ms,
                )?)
                .map_err(|e| anyhow::anyhow!("{e}"))?,
                interpretation: crate::pool::InterpretationResponse::from(&record.interpretation),
                record,
            }),
        )?);
        served.push(row);
    }

    let summary = address_pool::pool_summary(snapshot, served, segment.truncated)?;
    Ok((rows, summary))
}

async fn get_address_activities(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
    Query(params): Query<ActivityParams>,
) -> ApiResult<CursorPaginatedResponse<ActivityResponse>> {
    validate_activity_filter(params.filter.as_deref())?;
    // Routing via the shared `is_ckb_address` (not an inline prefix check) so
    // every address entry point shares one case-handling rule.
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid address: {}", e)))?
    } else {
        parse_hash32(&addr, "address/lock script hash")?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "activity cursor")?;

    let filter = params.filter.clone();
    let store = state.store.clone();
    let network = state.ckb_network.clone();
    let lock32: [u8; 32] = lock_hash.as_slice().try_into().map_err(|_| {
        ApiError::internal(format!(
            "address activities expect a 32-byte lock hash, got {} bytes",
            lock_hash.len()
        ))
    })?;
    // Pool rows are a page-one-only segment: a pool transaction can only land
    // in a future block, so it is later than every committed transaction in
    // canonical order. Keeping it out of cursors is what keeps pagination
    // stable while the pool churns.
    let pool_snapshot = if cursor.is_none() {
        Some(state.pool_mirror.load())
    } else {
        None
    };
    let (next_cursor, activities, pool, total) =
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let results = list_canonical_activities_page(
                store.as_ref(),
                &lock_hash,
                limit + 1,
                cursor,
                filter.as_deref(),
            )?;

            let has_more = results.len() > limit;
            let page: Vec<_> = results.into_iter().take(limit).collect();

            // The total is the unfiltered activity count, read from the same
            // pinned view as the page; a filtered feed has no stored count.
            let total = if matches!(filter.as_deref(), None | Some("all")) {
                let total = store.address_activity_count(&lock32)?;
                ensure_activity_total_covers_page_one(
                    &lock32,
                    total,
                    cursor,
                    page.len(),
                    has_more,
                )?;
                Some(total)
            } else {
                None
            };

            let next_cursor = if has_more {
                page.last()
                    .map(|actions| format!("{}:{}", actions.block_number, actions.tx_index))
            } else {
                None
            };

            let mut script_info_cache = HashMap::new();
            let mut item_cache = ItemDeltaCache::default();
            let mut address_cache = HashMap::new();

            let (mut activities, pool) = match pool_snapshot.as_deref() {
                Some(snapshot) => {
                    let (rows, summary) = build_pool_activity_rows(
                        store.as_ref(),
                        &network,
                        snapshot,
                        &lock32,
                        filter.as_deref(),
                        &mut script_info_cache,
                        &mut item_cache,
                        &mut address_cache,
                    )?;
                    (rows, Some(summary))
                }
                None => (Vec::new(), None),
            };

            for actions in &page {
                let Some(participant) = actions.participants.iter().find(|p| p.id.matches(&lock32))
                else {
                    continue;
                };
                activities.push(build_activity_response(
                    store.as_ref(),
                    &network,
                    actions,
                    participant,
                    &mut script_info_cache,
                    &mut item_cache,
                    &mut address_cache,
                    None,
                )?);
            }

            Ok((next_cursor, activities, pool, total))
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // `total` is the address's activity count (`address_activity_count`):
    // its transactions minus its cellbase transactions, which are never
    // persisted in CF_TX_ACTIONS (W5). It is NOT `AddressBalance.txs_count`,
    // which also counts cellbases (mainnet block 12000000's cellbase-output
    // address: 4_727_769 transactions against 1_682 activities). Committed
    // rows only; pool rows are reported beside it in `pool`.
    let response = match total {
        Some(total) => CursorPaginatedResponse::new(activities, total, limit as i64, next_cursor),
        None => CursorPaginatedResponse::without_total(activities, limit as i64, next_cursor),
    };
    ok(match pool {
        Some(pool) => response.with_pool(pool),
        None => response,
    })
}

/// Page one of an unfiltered address feed starts at the newest activity, so
/// the persisted total must cover its committed rows, and equal them when
/// there is no next page. Anything else means the counter and the rows it
/// counts disagree: an error naming the address, never a clamped total.
fn ensure_activity_total_covers_page_one(
    lock_hash: &[u8; 32],
    total: i64,
    cursor: Option<(i64, i32)>,
    committed_rows: usize,
    has_more: bool,
) -> anyhow::Result<()> {
    if cursor.is_some() {
        return Ok(());
    }
    let committed_rows = i64::try_from(committed_rows)?;
    if committed_rows > total || (!has_more && committed_rows != total) {
        anyhow::bail!(
            "address 0x{} activity total {} disagrees with page one: {} committed rows, has_more={}",
            hex::encode(lock_hash),
            total,
            committed_rows,
            has_more
        );
    }
    Ok(())
}

/// Cellbase actions are intentionally not materialized. Reuse the indexer's
/// interpreter over the node's exact outputs when a block/detail requests one.
fn canonical_transaction_actions(
    store: &CkbadgerStore,
    node: Option<&ckb_store_reader::CkbChainReader>,
    block: i64,
    index: i32,
    entry: &ckbadger_store::TxIndexEntry,
) -> anyhow::Result<TxActions> {
    if !entry.is_cellbase {
        return store
            .get_tx_actions(block, index, &entry.tx_hash)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "canonical activity missing at block={block} index={index} tx=0x{}",
                    hex::encode(entry.tx_hash)
                )
            });
    }
    use ckb_types::prelude::*;
    let node = node.ok_or_else(|| {
        anyhow::anyhow!("cellbase activity at block {block} requires CKB RocksDB reader")
    })?;
    let tx = node.get_transaction(&entry.tx_hash).ok_or_else(|| {
        anyhow::anyhow!(
            "cellbase transaction 0x{} at block {block} missing from CKB RocksDB",
            hex::encode(entry.tx_hash)
        )
    })?;
    anyhow::ensure!(
        tx.is_cellbase(),
        "indexed cellbase 0x{} is not cellbase in node bytes",
        hex::encode(entry.tx_hash)
    );
    anyhow::ensure!(
        tx.outputs().len() == tx.outputs_data().len(),
        "cellbase output/data length mismatch at block {block}"
    );
    let header = store
        .get_block_header(block)?
        .ok_or_else(|| anyhow::anyhow!("cellbase block {block} header missing"))?;
    let cells = tx
        .outputs()
        .into_iter()
        .zip(tx.outputs_data())
        .map(|(output, data)| -> anyhow::Result<_> {
            let lock = output.lock();
            let capacity: u64 = output.capacity().unpack();
            let type_script = output.type_().to_opt().map(|script| {
                (
                    script.code_hash().as_slice().to_vec(),
                    i16::from(script.hash_type().as_slice()[0]),
                    script.args().raw_data().to_vec(),
                )
            });
            let data = data.raw_data();
            let data_hash = ckb_types::packed::CellOutput::calc_data_hash(&data)
                .as_slice()
                .to_vec();
            let cell = crate::pool::ResolvedCell::new(
                i64::try_from(capacity)?,
                lock.code_hash().as_slice().to_vec(),
                i16::from(lock.hash_type().as_slice()[0]),
                lock.args().raw_data().to_vec(),
                type_script,
                data.to_vec(),
            )
            .map_err(anyhow::Error::msg)?;
            Ok((cell, data_hash))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let outputs = cells
        .iter()
        .map(|(cell, data_hash)| -> anyhow::Result<_> {
            Ok(ckbadger_indexer::db::OutputCellView {
                capacity: cell.capacity,
                lock_code_hash: &cell.lock_code_hash,
                lock_hash_type: cell.lock_hash_type,
                lock_args: &cell.lock_args,
                lock_script_hash: &cell.lock_script_hash,
                type_code_hash: cell.type_code_hash.as_deref(),
                type_hash_type: cell.type_hash_type,
                type_args: cell.type_args.as_deref(),
                type_script_hash: cell.type_script_hash.as_deref(),
                data_hash,
                data_size: i32::try_from(cell.data.len())?,
                data: &cell.data,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let view = ckbadger_indexer::db::TxView {
        tx_hash: &entry.tx_hash,
        block_hash: &header.hash,
        tx_index: index,
        block_number: block,
        timestamp: entry.timestamp,
        is_cellbase: true,
        dotbit_action: None,
        inputs: vec![],
        outputs,
    };
    ckbadger_indexer::db::build_tx_actions_for_block(&[view], &[])?
        .pop()
        .ok_or_else(|| anyhow::anyhow!("cellbase interpreter returned no action at block {block}"))
}

async fn get_transaction_activity(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Option<GlobalActivityResponse>> {
    let hash = parse_hash32(&hash, "transaction hash")?;
    let (block, index, tx) = state
        .store
        .get_tx_by_hash(&hash)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Committed transaction not found"))?;
    let actions =
        canonical_transaction_actions(&state.store, state.ckb_store.as_deref(), block, index, &tx)
            .map_err(|e| ApiError::internal(e.to_string()))?;
    let activity = build_global_activity_response(
        &state.store,
        &state.append_only_store,
        &state.ckb_network,
        &actions,
        &mut HashMap::new(),
        &mut HashMap::new(),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    ok(Some(activity))
}

async fn get_global_activities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ActivityParams>,
) -> ApiResult<CursorPaginatedResponse<GlobalActivityResponse>> {
    validate_global_activity_filter(params.filter.as_deref())?;

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor = parse_optional_block_tx_cursor(params.cursor.as_deref(), "activity cursor")?;

    let block_number = params
        .block_number
        .map(|n| crate::utils::validate_block_number(n, "blockNumber"))
        .transpose()?;
    let ascending = match (params.order.as_deref(), block_number) {
        (None | Some("desc"), _) => false,
        (Some("asc"), Some(_)) => true,
        (Some("asc"), None) => {
            return Err(ApiError::bad_request(
                "order=asc reads one block's activities in block order; it needs blockNumber",
            ))
        }
        (Some(other), _) => {
            return Err(ApiError::bad_request(format!(
                "invalid order '{other}'; expected asc or desc"
            )))
        }
    };
    if let (Some(block), Some((cursor_block, _))) = (block_number, cursor) {
        if block != cursor_block {
            return Err(ApiError::bad_request(
                "activity cursor belongs to another block",
            ));
        }
    }
    let store = state.store.clone();
    let ao_store = state.append_only_store.clone();
    let network = state.ckb_network.clone();
    let node = state.ckb_store.clone();
    let filter = params.filter.clone();
    let (next_cursor, activities) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let results = if let Some(block) = block_number {
            let mut rows = Vec::new();
            let mut txs = store.list_block_txs(block)?;
            if !ascending {
                txs.reverse();
            }
            for (index, tx) in txs {
                // The cursor is the last row served: continue past it in the
                // page's own direction.
                if cursor.is_some_and(|(_, last)| {
                    if ascending {
                        index <= last
                    } else {
                        index >= last
                    }
                }) {
                    continue;
                }
                let actions =
                    canonical_transaction_actions(&store, node.as_deref(), block, index, &tx)?;
                if matches_global_activity_filter(&actions, filter.as_deref()) {
                    rows.push(actions);
                }
                if rows.len() > limit {
                    break;
                }
            }
            rows
        } else {
            list_canonical_global_actions_page(
                store.as_ref(),
                limit + 1,
                cursor,
                filter.as_deref(),
            )?
        };

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

    /// Activity type/lock call names and participant lock kinds read the one
    /// registry name rule `/cells` uses (`utils::registered_script_name`):
    /// trimmed, `None` for an unregistered or "Unknown" reference; and one
    /// response reads each code hash once.
    #[test]
    fn activity_script_names_are_the_registered_names_read_once_per_response() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let named = [0x01; 32];
        let unknown = [0x02; 32];
        let mut batch = ckbadger_store::StoreBatch::new(&store);
        for (code_hash, name) in [(named, " JoyID "), (unknown, "Unknown")] {
            batch.put_script_info(
                &code_hash,
                &ckbadger_store::types::ScriptInfo {
                    code_hash: code_hash.to_vec(),
                    name: Some(name.to_string()),
                    ..Default::default()
                },
            );
        }
        batch.commit().unwrap();

        let mut cache = ScriptNameCache::new();
        for code_hash in [named, unknown, [0x03; 32]] {
            assert_eq!(
                cached_script_name(&store, &mut cache, &code_hash).unwrap(),
                registered_script_name(&store, &code_hash).unwrap()
            );
        }
        assert_eq!(
            cached_script_name(&store, &mut cache, &named).unwrap(),
            Some("JoyID".to_string())
        );
        assert_eq!(
            cached_script_name(&store, &mut cache, &unknown).unwrap(),
            None
        );
        store.reset_read_call_counters();
        cached_script_name(&store, &mut cache, &named).unwrap();
        assert_eq!(
            store.read_call_counts(),
            (0, 0),
            "a cached name is not re-read"
        );
    }

    #[test]
    fn discrete_item_delta_requires_exact_unit_magnitude() {
        let item = ItemDelta {
            item_id: vec![0xAA; 32],
            kind: ItemKind::Object,
            magnitude: 2,
            negative: false,
        };
        let err = discrete_item_delta(&item).unwrap_err().to_string();
        assert!(err.contains("magnitude=2"));
        assert!(err.contains("expected 1"));
    }

    #[test]
    fn token_metadata_read_error_is_not_an_unnamed_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let token_id = [0xAA; 32];
        store
            .put_cf(store.cf_tokens(), &token_id, b"corrupt-token-row")
            .unwrap();
        let mut cache = HashMap::new();
        let error = lookup_token_info(&store, &mut cache, &token_id)
            .unwrap_err()
            .to_string();
        assert!(error.contains(&hex::encode(token_id)), "{error}");
        assert!(
            cache.is_empty(),
            "failed reads must not poison the metadata cache"
        );
    }

    #[test]
    fn discrete_item_delta_preserves_sign() {
        let mut item = ItemDelta {
            item_id: vec![0xBB; 32],
            kind: ItemKind::Identity(ckbadger_store::types::IdentityStandard::DotCell),
            magnitude: 1,
            negative: false,
        };
        assert_eq!(discrete_item_delta(&item).unwrap(), 1);
        item.negative = true;
        assert_eq!(discrete_item_delta(&item).unwrap(), -1);
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

    // Integration tests for activities are in crates/api/tests/api_activities.rs

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
    fn lock_args_decoders_follow_the_script_registry() {
        // Every ordinary-family code hash in the table is a canonical_ref_hash
        // of the registry file named beside it.
        let registry = |toml: &str| -> Vec<Vec<u8>> {
            toml.lines()
                .filter_map(|line| line.trim().strip_prefix("canonical_ref_hash = \""))
                .map(|rest| parse_hex_code_hash(rest.trim_end_matches('"')))
                .collect()
        };
        for (toml, family) in [
            (
                include_str!("../../../../docs/metadata/scripts/default-lock.toml"),
                "secp256k1_blake160",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/default-multisig.toml"),
                "multisig",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/anyone-can-pay-lock.toml"),
                "anyone_can_pay",
            ),
            (
                include_str!("../../../../docs/metadata/scripts/omni-lock.toml"),
                "omnilock",
            ),
        ] {
            let hashes = registry(toml);
            assert!(!hashes.is_empty(), "{family}");
            let table: Vec<&Vec<u8>> = LOCK_ARGS_DECODERS
                .iter()
                .filter(|(_, decoder)| {
                    let probe = decoder(&[0u8; 22]).or_else(|| decoder(&[0u8; 20]));
                    probe.is_some_and(|value| value["lock"] == family)
                })
                .map(|(hash, _)| hash)
                .collect();
            for hash in &hashes {
                assert!(
                    table.contains(&hash),
                    "{family}: 0x{} not decoded",
                    hex::encode(hash)
                );
            }
            assert_eq!(
                table.len(),
                hashes
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                "{family}"
            );
        }
    }

    #[test]
    fn ordinary_lock_args_decode_by_their_specified_layouts() {
        assert_eq!(
            decode_multisig_args(&[[7u8; 20].as_slice(), &5u64.to_le_bytes()].concat()).unwrap()
                ["since"],
            5
        );
        assert!(decode_multisig_args(&[7u8; 21]).is_none());
        let acp = decode_anyone_can_pay_args(&[[1u8; 20].as_slice(), &[8, 2]].concat()).unwrap();
        assert_eq!(acp["minimumCkbExponent"], 8);
        assert_eq!(acp["minimumUdtExponent"], 2);
        assert!(decode_anyone_can_pay_args(&[1u8; 23]).is_none());
        // Omnilock: the flags must account for every byte after the auth.
        let plain = [[0u8].as_slice(), &[9u8; 20], &[0]].concat();
        assert_eq!(decode_omnilock_args(&plain).unwrap()["omnilockFlags"], 0);
        assert!(decode_omnilock_args(&[plain.as_slice(), &[1]].concat()).is_none());
        let acp_mode = [[0x06u8].as_slice(), &[9u8; 20], &[0x2], &[3, 4]].concat();
        let decoded = decode_omnilock_args(&acp_mode).unwrap();
        assert_eq!(decoded["authKind"], "ckb_multisig");
        assert_eq!(decoded["minimumUdtExponent"], 4);
        assert!(decode_secp256k1_blake160_args(&[0u8; 21]).is_none());
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

    /// Real mainnet swap intent args (tx 0x44f659be…, block 13,845,652).
    const REAL_SWAP_INTENT_ARGS: &str = "0xbefc0a6053441e9bcba6d3f6c1599c37a1d8187a235edb927fc68f446e06f2e677fb52aa7f158ae800000000000000000000000000000000030180ea822b000000000000000000000000324f4251220000000000000000000000";

    /// Real mainnet AddLiquidity intent args (tx 0x18d1b37e…, block 14,046,271).
    const REAL_ADD_LIQUIDITY_INTENT_ARGS: &str = "0x0001d85947f67df16556a1caef3b7f939a69fb2329273406698f36e9bdf46db404176859b0ba3a6b00000000000000000000000000000000013a219800000000000000000000000000805e9700000000000000000000000000506c0300000000000000000000000000ee670300000000000000000000000000";

    /// Real mainnet RemoveLiquidity intent args (tx 0x416ed0a3…, block 20,003,047).
    const REAL_REMOVE_LIQUIDITY_INTENT_ARGS: &str = "0xc41696293f5b16b471f9116631da82a4102c5b01b82e9073fee07b9caf625f0be45d3ec061be221200000000000000000100000000000000025b4ff3776d2f00000000000000000000b7d95acc3505000000000000000000001b35f86b53d501000000000000000000";

    #[test]
    fn test_decode_utxoswap_intent_swap() {
        let args = parse_hex_code_hash(REAL_SWAP_INTENT_ARGS);
        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "SwapExactInputForOutput");
        assert_eq!(
            result["poolTypeHash"],
            "0x235edb927fc68f446e06f2e677fb52aa7f158ae8"
        );
        assert_eq!(result["assetInIndex"], 1);
        assert_eq!(result["amountIn"], "730000000");
        assert_eq!(result["amountOutMin"], "147392188210");
        assert!(result.get("assetX").is_none());
    }

    /// The API's live display used to re-implement the decode and carried the
    /// identical bug: the 90-byte swap layout applied to a 121-byte
    /// AddLiquidity payload, yielding 2^127-scale amounts. It now shares the
    /// one decoder in `ckbadger-common`.
    #[test]
    fn test_decode_utxoswap_intent_add_liquidity() {
        let args = parse_hex_code_hash(REAL_ADD_LIQUIDITY_INTENT_ARGS);
        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "AddLiquidity");
        assert_eq!(result["desiredX"], "9969978");
        assert_eq!(result["minX"], "9920128");
        assert_eq!(result["desiredY"], "224336");
        assert_eq!(result["minY"], "223214");

        assert!(result.get("amountIn").is_none());
        assert!(result.get("amountOutMin").is_none());
        assert!(result.get("assetInIndex").is_none());
        assert!(
            !result
                .to_string()
                .contains("170141183460469231731687303715884144673"),
            "old 2^127-scale garbage resurfaced: {result}"
        );
    }

    #[test]
    fn test_decode_utxoswap_intent_remove_liquidity() {
        let args = parse_hex_code_hash(REAL_REMOVE_LIQUIDITY_INTENT_ARGS);
        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["intentType"], "RemoveLiquidity");
        assert_eq!(result["lpAmount"], "52147210375003");
        assert_eq!(result["minX"], "5728619911607");
        assert_eq!(result["minY"], "516029247141147");
        assert!(result.get("amountIn").is_none());
    }

    #[test]
    fn test_decode_utxoswap_intent_create_pool() {
        // Real mainnet CreatePool intent args (tx 0xdd8b76a8…, block 13,372,041).
        let args = parse_hex_code_hash("0x4d93a976fd4eb7a6349c020fadc3ef65834701dc000000000000000000000000000000000000000000000000000000000100000000000000001e000000000000000000000000000000000000000000000000000000000000000061bd91b121e5b7bbf9ccb4bc46c3106ac69c2dfd7b1c1143c4b4fdb33fd6182600e40b5402000000000000000000000000e40b54020000000000000000000000");

        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["protocol"], "utxoswap");
        assert_eq!(result["intentType"], "CreatePool");
        assert_eq!(result["totalFeeRate"], 30);
        assert_eq!(result["amountX"], "10000000000");
        assert_eq!(result["amountY"], "10000000000");
        assert_eq!(
            result["assetY"],
            "0x61bd91b121e5b7bbf9ccb4bc46c3106ac69c2dfd7b1c1143c4b4fdb33fd61826"
        );
        // CreatePool no longer carries bogus zero-valued swap fields.
        assert!(result.get("amountIn").is_none());
        assert!(result.get("assetInIndex").is_none());
    }

    #[test]
    fn test_decode_utxoswap_intent_too_short() {
        // Below the 57-byte shared header there is nothing trustworthy to read.
        let args = vec![0u8; 56];
        assert!(decode_utxoswap_intent_args(&args).is_none());
    }

    /// A recognised type with an unexpected payload length must be reported as
    /// unparsed rather than decoded with some other type's layout.
    #[test]
    fn test_decode_utxoswap_intent_unexpected_length_is_unparsed() {
        let mut args = parse_hex_code_hash(REAL_SWAP_INTENT_ARGS);
        args[56] = 1; // claim AddLiquidity on a 90-byte payload
        let result = decode_utxoswap_intent_args(&args).unwrap();
        assert_eq!(result["intentType"], "AddLiquidity");
        assert_eq!(result["payloadUnparsed"], true);
        assert_eq!(result["argsLen"], 90);
        assert!(result.get("amountIn").is_none());
        assert!(result.get("desiredX").is_none());
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
            collection: None,
            class: "object",
            composition: None,
            role: None,
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
            standard: ckbadger_store::types::IdentityStandard::DotCell.asset_standard(),
        };
        let json = serde_json::to_string(&identity).unwrap();
        assert!(
            json.contains("\"identityId\""),
            "missing identityId: {}",
            json
        );
        assert!(
            json.contains("\"standard\":\"dotcell\""),
            "missing standard: {}",
            json
        );
        assert!(
            json.contains("\"kind\":\"identity\""),
            "missing kind: {}",
            json
        );

        // A token delta > i128::MAX must render as its exact decimal string. This is the
        // exact expression convert_item_delta uses for a token (`format!("{sign}{magnitude}")`),
        // proving no truncation at the API/JSON boundary. (The indexer-side regression test
        // pins that `magnitude` reaches here un-wrapped.)
        let magnitude: u128 = 222_044_604_925_031_325_468_940_491_728_862_838_784; // 2.22e38
        let negative = false;
        let big_token = ItemDeltaResponse::Token {
            type_script_hash: "0xdd".to_string(),
            delta: format!("{}{}", if negative { "-" } else { "" }, magnitude),
            symbol: None,
            decimals: None,
        };
        let json = serde_json::to_string(&big_token).unwrap();
        assert!(
            json.contains("\"222044604925031325468940491728862838784\""),
            "big token delta must serialize as exact decimal string: {}",
            json
        );
    }
}
